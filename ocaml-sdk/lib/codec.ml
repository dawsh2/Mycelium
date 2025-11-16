open Stdlib

let encode_frame ~type_id ~payload =
  let payload_len = Bytes.length payload in
  let buf = Bytes.create (6 + payload_len) in
  let offset = Runtime.Primitive.encode_u16 buf 0 type_id in
  let offset = Runtime.Primitive.encode_u32 buf offset payload_len in
  Bytes.blit payload 0 buf offset payload_len;
  buf

let rec read_exact fd buf offset remaining =
  if remaining = 0 then ()
  else
    let bytes_read = Unix.read fd buf offset remaining in
    if bytes_read = 0 then raise End_of_file
    else read_exact fd buf (offset + bytes_read) (remaining - bytes_read)

let read_frame fd =
  let header = Bytes.create 6 in
  try
    read_exact fd header 0 6;
    let type_id, offset = Runtime.Primitive.decode_u16 header 0 in
    let payload_len, _ = Runtime.Primitive.decode_u32 header offset in
    if payload_len < 0 || payload_len > (4 * 1024 * 1024) then
      invalid_arg "Codec.read_frame: invalid payload length";
    let payload = Bytes.create payload_len in
    read_exact fd payload 0 payload_len;
    Some (type_id, payload)
  with End_of_file -> None

let send_handshake fd digest =
  if Bytes.length digest <> 32 then
    invalid_arg "Codec.send_handshake: digest must be 32 bytes";
  let buf = Bytes.create 34 in
  let offset = Runtime.Primitive.encode_u16 buf 0 32 in
  Bytes.blit digest 0 buf offset 32;
  ignore (Unix.write fd buf 0 34)
