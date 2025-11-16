[@@@ocaml.warning "-33"]

module TextMessage = Mycelium.Messages.TextMessage

let socket_path = Sys.getenv "MYCELIUM_SOCKET"
let digest_hex = Sys.getenv "MYCELIUM_SCHEMA_DIGEST"
let output_path = Sys.getenv "MYCELIUM_TEST_OUTPUT"

let digest_from_hex hex =
  let len = String.length hex in
  let bytes = Bytes.create (len / 2) in
  for i = 0 to (len / 2) - 1 do
    let byte = int_of_string ("0x" ^ String.sub hex (2 * i) 2) in
    Bytes.set bytes i (Char.chr byte)
  done;
  bytes

let () =
  let schema_digest = digest_from_hex digest_hex in
  let transport =
    Mycelium.Transport.connect_unix ~socket_path ~schema_digest in
  let publisher =
    Mycelium.Transport.publisher transport ~type_id:TextMessage.type_id in
  let subscriber =
    Mycelium.Transport.subscribe transport ~type_id:TextMessage.type_id in

  let outbound =
    {
      TextMessage.sender = "ocaml-writer";
      content = "hello-from-ocaml";
      timestamp = Int64.of_float (Unix.time ());
    }
  in
  Mycelium.Transport.send publisher (TextMessage.encode outbound);

  let inbound =
    Mycelium.Transport.recv subscriber |> TextMessage.decode in
  let json =
    Yojson.Basic.(
      `Assoc
        [ ( "outbound",
            `Assoc
              [ ("sender", `String outbound.sender);
                ("content", `String outbound.content);
                ("timestamp", `Float (Int64.to_float outbound.timestamp)) ] );
          ( "inbound",
            `Assoc
              [ ("sender", `String inbound.sender);
                ("content", `String inbound.content);
                ("timestamp", `Float (Int64.to_float inbound.timestamp)) ] )
        ])
  in
  let oc = open_out output_path in
  Yojson.Basic.pretty_to_channel oc json;
  close_out oc;
  Mycelium.Transport.close transport
