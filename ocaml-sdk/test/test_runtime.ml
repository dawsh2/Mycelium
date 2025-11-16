open Alcotest

let test_fixed_str_roundtrip () =
  let buf = Bytes.create 40 in
  let offset = Mycelium.Runtime.FixedStr.encode buf 0 "hello" ~capacity:32 in
  check int "offset" 40 offset;
  let decoded, _ = Mycelium.Runtime.FixedStr.decode buf 0 ~capacity:32 in
  check string "value" "hello" decoded

let test_fixed_bytes_roundtrip () =
  let buf = Bytes.create 16 in
  let payload = Bytes.of_string "0123456789abcdef" in
  let offset =
    Mycelium.Runtime.FixedBytes.encode buf 0 payload ~length:(Bytes.length payload)
  in
  check int "offset" 16 offset;
  let decoded, _ =
    Mycelium.Runtime.FixedBytes.decode buf 0 ~length:(Bytes.length payload)
  in
  check string "bytes" (Bytes.to_string payload) (Bytes.to_string decoded)

let tests =
  [ "fixed-str", `Quick, test_fixed_str_roundtrip;
    "fixed-bytes", `Quick, test_fixed_bytes_roundtrip
  ]

let () = Alcotest.run "runtime" [ "runtime", tests ]
