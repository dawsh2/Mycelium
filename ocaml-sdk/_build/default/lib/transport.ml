[@@@ocaml.warning "-32-33-39-69"]

open Stdlib

type subscription = {
  queue : bytes Queue.t;
  mutex : Mutex.t;
  cond : Condition.t;
  mutable active : bool;
}

type t = {
  socket : Unix.file_descr;
  send_mutex : Mutex.t;
  subscribers : (int, subscription list) Hashtbl.t;
  subscribers_mutex : Mutex.t;
  mutable closed : bool;
  reader_thread : Thread.t;
}

type publisher = { transport : t; type_id : int }
type subscriber = { transport : t; sub : subscription; type_id : int }

let write_all fd buf =
  let len = Bytes.length buf in
  let rec loop offset remaining =
    if remaining = 0 then ()
    else
      let written = Unix.write fd buf offset remaining in
      if written = 0 then raise End_of_file
      else loop (offset + written) (remaining - written)
  in
  loop 0 len

let register_subscription t type_id subscription =
  Mutex.lock t.subscribers_mutex;
  let existing = Hashtbl.find_opt t.subscribers type_id |> Option.value ~default:[] in
  Hashtbl.replace t.subscribers type_id (subscription :: existing);
  Mutex.unlock t.subscribers_mutex

let unregister_subscription t type_id subscription =
  Mutex.lock t.subscribers_mutex;
  let updated =
    Hashtbl.find_opt t.subscribers type_id
    |> Option.map (List.filter (fun s -> s != subscription))
    |> Option.value ~default:[]
  in
  if updated = [] then Hashtbl.remove t.subscribers type_id
  else Hashtbl.replace t.subscribers type_id updated;
  Mutex.unlock t.subscribers_mutex

let dispatch_frame t type_id payload =
  Mutex.lock t.subscribers_mutex;
  let targets = Hashtbl.find_opt t.subscribers type_id |> Option.value ~default:[] in
  List.iter
    (fun sub ->
      Mutex.lock sub.mutex;
      if sub.active then (
        Queue.add (Bytes.copy payload) sub.queue;
        Condition.signal sub.cond);
      Mutex.unlock sub.mutex)
    targets;
  Mutex.unlock t.subscribers_mutex

let rec reader_loop t =
  match Codec.read_frame t.socket with
  | None -> t.closed <- true
  | Some (type_id, payload) ->
      dispatch_frame t type_id payload;
      reader_loop t

let start_reader t = Thread.create reader_loop t

let make_transport socket schema_digest =
  Codec.send_handshake socket schema_digest;
  let transport =
    {
      socket;
      send_mutex = Mutex.create ();
      subscribers = Hashtbl.create 8;
      subscribers_mutex = Mutex.create ();
      closed = false;
      reader_thread = Thread.self ();
    }
  in
  let reader = start_reader transport in
  { transport with reader_thread = reader }

let connect_unix ~socket_path ~schema_digest =
  let socket = Unix.socket Unix.PF_UNIX Unix.SOCK_STREAM 0 in
  let addr = Unix.ADDR_UNIX socket_path in
  Unix.connect socket addr;
  make_transport socket schema_digest

let connect_tcp ~host ~port ~schema_digest =
  let socket = Unix.socket Unix.PF_INET Unix.SOCK_STREAM 0 in
  let inet_addr = (Unix.gethostbyname host).Unix.h_addr_list.(0) in
  let addr = Unix.ADDR_INET (inet_addr, port) in
  Unix.connect socket addr;
  make_transport socket schema_digest

let close t =
  if not t.closed then (
    t.closed <- true;
    (try Unix.shutdown t.socket Unix.SHUTDOWN_ALL with _ -> ());
    (try Unix.close t.socket with _ -> ());
    Mutex.lock t.subscribers_mutex;
    Hashtbl.iter
      (fun _ subs ->
        List.iter
          (fun sub ->
            Mutex.lock sub.mutex;
            sub.active <- false;
            Condition.broadcast sub.cond;
            Mutex.unlock sub.mutex)
          subs)
      t.subscribers;
    Mutex.unlock t.subscribers_mutex)

let publisher t ~type_id = { transport = t; type_id }

let send (pub : publisher) payload =
  if pub.transport.closed then invalid_arg "Transport closed";
  let frame = Codec.encode_frame ~type_id:pub.type_id ~payload in
  Mutex.lock pub.transport.send_mutex;
  (try write_all pub.transport.socket frame with e ->
     Mutex.unlock pub.transport.send_mutex; raise e);
  Mutex.unlock pub.transport.send_mutex

let subscribe t ~type_id =
  let subscription =
    {
      queue = Queue.create ();
      mutex = Mutex.create ();
      cond = Condition.create ();
      active = true;
    }
  in
  register_subscription t type_id subscription;
  { transport = t; sub = subscription; type_id }

let rec wait_for_message sub ~timeout =
  let rec check_queue () =
    if not sub.sub.active then raise End_of_file
    else if Queue.is_empty sub.sub.queue then None
    else Some (Queue.take sub.sub.queue)
  in
  let rec wait_blocking () =
    Mutex.lock sub.sub.mutex;
    let result = check_queue () in
    match result with
    | Some msg ->
        Mutex.unlock sub.sub.mutex;
        Some msg
    | None ->
        Condition.wait sub.sub.cond sub.sub.mutex;
        Mutex.unlock sub.sub.mutex;
        wait_blocking ()
    | exception End_of_file ->
        Mutex.unlock sub.sub.mutex;
        raise End_of_file
  in
  let rec wait_with_timeout remaining =
    Mutex.lock sub.sub.mutex;
    let result = check_queue () in
    match result with
    | Some msg ->
        Mutex.unlock sub.sub.mutex;
        Some msg
    | None when Float.(remaining <= 0.) ->
        Mutex.unlock sub.sub.mutex;
        None
    | None ->
        Mutex.unlock sub.sub.mutex;
        let sleep = Float.min remaining 0.05 in
        Thread.delay sleep;
        wait_with_timeout Float.(remaining -. sleep)
    | exception End_of_file ->
        Mutex.unlock sub.sub.mutex;
        raise End_of_file
  in
  if Float.is_infinite timeout then wait_blocking () else wait_with_timeout timeout

let recv sub =
  match wait_for_message sub ~timeout:infinity with
  | Some msg -> msg
  | None -> raise End_of_file

let recv_timeout sub timeout = wait_for_message sub ~timeout

let () = ()
