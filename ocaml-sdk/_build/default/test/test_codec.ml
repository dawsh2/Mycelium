let test_encode_decode_frame () =
  let payload = Bytes.of_string "hello" in
  let frame = Mycelium.Codec.encode_frame ~type_id:42 ~payload in
  (* fake socket via Unix.pipe *)
  let r, w = Unix.pipe () in
  (* send frame and attempt to read back using codec *)
  let sent = Unix.write w frame 0 (Bytes.length frame) in
  Alcotest.(check int) "sent length" (Bytes.length frame) sent;
  Unix.close w;
  match Mycelium.Codec.read_frame r with
  | Some (type_id, received) ->
      Alcotest.(check int) "type" 42 type_id;
      Alcotest.(check string) "payload" "hello" (Bytes.to_string received)
  | None -> Alcotest.fail "expected frame"

let tests =
  [ "encode/decode frame", `Quick, test_encode_decode_frame ]

let () = Alcotest.run "codec" [ "codec", tests ]
