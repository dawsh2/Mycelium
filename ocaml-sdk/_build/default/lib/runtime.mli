module Primitive : sig
  val encode_u8 : bytes -> int -> int -> int
  val decode_u8 : bytes -> int -> int * int

  val encode_u16 : bytes -> int -> int -> int
  val decode_u16 : bytes -> int -> int * int

  val encode_u32 : bytes -> int -> int -> int
  val decode_u32 : bytes -> int -> int * int

  val encode_u64 : bytes -> int -> int64 -> int
  val decode_u64 : bytes -> int -> int64 * int

  val encode_i32 : bytes -> int -> int -> int
  val decode_i32 : bytes -> int -> int * int

  val encode_f64 : bytes -> int -> float -> int
  val decode_f64 : bytes -> int -> float * int
end

module FixedBytes : sig
  val encode : bytes -> int -> bytes -> length:int -> int
  val decode : bytes -> int -> length:int -> bytes * int
end

module FixedStr : sig
  val encode : bytes -> int -> string -> capacity:int -> int
  val decode : bytes -> int -> capacity:int -> string * int
end

module FixedVecBytes : sig
  val encode_list :
    bytes -> int -> bytes list -> element_size:int -> capacity:int -> int

  val decode_list :
    bytes -> int -> element_size:int -> capacity:int -> bytes list * int

  val encode_bytes : bytes -> int -> bytes -> capacity:int -> int
  val decode_bytes : bytes -> int -> capacity:int -> bytes * int
end
