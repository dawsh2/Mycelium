val encode_frame : type_id:int -> payload:bytes -> bytes
val read_frame : Unix.file_descr -> (int * bytes) option
val send_handshake : Unix.file_descr -> bytes -> unit
