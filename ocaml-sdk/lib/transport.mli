type t

type publisher

type subscriber

val connect_unix : socket_path:string -> schema_digest:bytes -> t
val connect_tcp : host:string -> port:int -> schema_digest:bytes -> t
val close : t -> unit

val publisher : t -> type_id:int -> publisher
val send : publisher -> bytes -> unit

val subscribe : t -> type_id:int -> subscriber
val recv : subscriber -> bytes
val recv_timeout : subscriber -> float -> bytes option
