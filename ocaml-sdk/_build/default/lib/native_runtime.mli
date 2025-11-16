type t
type subscription

val create : ?schema_digest:bytes -> unit -> t
val close : t -> unit
val publish_raw : t -> int -> bytes -> unit
val subscribe_raw : t -> int -> (bytes -> unit) -> subscription
val unsubscribe : subscription -> unit

val publish : t -> type_id:int -> ('msg -> bytes) -> 'msg -> unit
val subscribe :
  t ->
  type_id:int ->
  decode:(bytes -> 'msg) ->
  callback:('msg -> unit) -> subscription
