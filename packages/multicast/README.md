## Multicast

Basic async multicast implementation using `tokio` and `protobuf`.

It supports:
- Sending and receiving messages of any arbitrary type that implements `Serialize` and `Deserialize`.
- TTL for messages, allow them to be re-broadcasted a limited number of times.
- Broadcasted acknowledgements to confirm message reception, suitable for downstream implementation of ack-based Uniform Reliable Broadcast.

> [!WARNING]
> This is currently in development and not guaranteed to work multi-platform.
>
> It is likely not to work on
> - Windows
> - MacOS with IPv6

### Pre-requisites

The `protobuf` crate requires `protoc` to be installed. See [Protocol Buffer Compiler Installation](https://grpc.io/docs/protoc-installation/) for more information.
