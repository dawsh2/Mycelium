## Mycelium

Mycelium is an actor runtime for Rust that provides a consistent message-passing API across different deployment topologies.  
Actors always communicate using messages, but the underlying transport adapts depending on where those actors run:

- **Single process**: messages are passed as `Arc<T>` references with no serialization.  
- **Same machine, multiple processes**: messages are serialized once and sent over Unix domain sockets.  
- **Across machines**: messages are serialized and transmitted over TCP.  

This approach allows the same actor code to run efficiently as either a monolith or as part of a distributed system, without requiring changes in application logic.  
