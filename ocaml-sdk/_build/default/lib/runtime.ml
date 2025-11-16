open Stdlib

module LE = EndianBytes.LittleEndian

module Primitive = struct
  let encode_u8 buf offset value =
    Bytes.set buf offset (Char.chr (value land 0xFF));
    offset + 1

  let decode_u8 buf offset =
    (Char.code (Bytes.get buf offset), offset + 1)

  let encode_u16 buf offset value =
    LE.set_int16 buf offset value;
    offset + 2

  let decode_u16 buf offset =
    (LE.get_int16 buf offset, offset + 2)

  let encode_u32 buf offset value =
    let value32 = Int32.of_int value in
    LE.set_int32 buf offset value32;
    offset + 4

  let decode_u32 buf offset =
    (Int32.to_int (LE.get_int32 buf offset), offset + 4)

  let encode_u64 buf offset value =
    LE.set_int64 buf offset value;
    offset + 8

  let decode_u64 buf offset =
    (LE.get_int64 buf offset, offset + 8)

  let encode_i32 buf offset value =
    LE.set_int32 buf offset (Int32.of_int value);
    offset + 4

  let decode_i32 buf offset =
    (Int32.to_int (LE.get_int32 buf offset), offset + 4)

  let encode_f64 buf offset value =
    let bits = Int64.bits_of_float value in
    LE.set_int64 buf offset bits;
    offset + 8

  let decode_f64 buf offset =
    let bits = LE.get_int64 buf offset in
    (Int64.float_of_bits bits, offset + 8)
end

module FixedBytes = struct
  let encode buf offset value ~length =
    if Bytes.length value <> length then
      invalid_arg "FixedBytes.encode: length mismatch";
    Bytes.blit value 0 buf offset length;
    offset + length

  let decode buf offset ~length =
    (Bytes.sub buf offset length, offset + length)
end

module FixedStr = struct
  let encode buf offset value ~capacity =
    let len = min (String.length value) capacity in
    ignore (Primitive.encode_u16 buf offset len);
    Bytes.fill buf (offset + 2) 6 '\000';
    Bytes.fill buf (offset + 8) capacity '\000';
    Bytes.blit_string value 0 buf (offset + 8) len;
    offset + 8 + capacity

  let decode buf offset ~capacity =
    let len, offset = Primitive.decode_u16 buf offset in
    let offset = offset + 6 in
    let raw = Bytes.sub buf offset capacity in
    let s = Bytes.sub_string raw 0 len in
    (s, offset + capacity)
end

module FixedVecBytes = struct
  let header_padding = 6

  let encode_list buf offset values ~element_size ~capacity =
    let count = List.length values in
    if count > capacity then invalid_arg "FixedVecBytes.encode_list: overflow";
    let offset = Primitive.encode_u16 buf offset count in
    Bytes.fill buf offset header_padding '\000';
    let offset = offset + header_padding in
    let total_bytes = element_size * capacity in
    Bytes.fill buf offset total_bytes '\000';
    List.iteri
      (fun idx value ->
        if Bytes.length value <> element_size then
          invalid_arg "FixedVecBytes.encode_list: element size mismatch";
        let start = offset + (idx * element_size) in
        Bytes.blit value 0 buf start element_size)
      values;
    offset + total_bytes

  let decode_list buf offset ~element_size ~capacity =
    let count, offset = Primitive.decode_u16 buf offset in
    if count > capacity then invalid_arg "FixedVecBytes.decode_list: overflow";
    let offset = offset + header_padding in
    let total_bytes = element_size * capacity in
    let values =
      let rec loop acc idx =
        if idx = count then List.rev acc
        else
          let start = offset + (idx * element_size) in
          let chunk = Bytes.sub buf start element_size in
          loop (chunk :: acc) (idx + 1)
      in
      loop [] 0
    in
    (values, offset + total_bytes)

  let encode_bytes buf offset value ~capacity =
    let count = Bytes.length value in
    if count > capacity then invalid_arg "FixedVecBytes.encode_bytes: overflow";
    let offset = Primitive.encode_u16 buf offset count in
    Bytes.fill buf offset header_padding '\000';
    let offset = offset + header_padding in
    Bytes.fill buf offset capacity '\000';
    Bytes.blit value 0 buf offset count;
    offset + capacity

  let decode_bytes buf offset ~capacity =
    let count, offset = Primitive.decode_u16 buf offset in
    if count > capacity then invalid_arg "FixedVecBytes.decode_bytes: overflow";
    let offset = offset + header_padding in
    let payload = Bytes.sub buf offset capacity in
    (Bytes.sub payload 0 count, offset + capacity)
end
