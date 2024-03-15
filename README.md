A naive implementation of distributed system to do arbitrary work.

Just an quick experiment to play with gRPC, with future plan to do some real work with it.

### Aspirations
- [X] Implement a distributed system that has no leaders; each node is equal.
- [X] Any of the nodes can receive a request, that if it can't handle, will forward
      to another node.
- [X] Communicate using gRPC protocol, with preset enum of message types.
- [ ] The system should be able to handle node failures, and continue to work.
- [X] The list of nodes should be stored as a min heap using the last called time
      as the key. This way, the node that has been idle the longest will be the
      first to receive a request. This could be tweaked to take into account the
      distance between nodes; but currently all nodes are assumed to be clustered in
      the same subnet and have the same latency.
- [ ] In the future, the nodes should discover each other using multicast, rather than
      a static list.

### Simple demo

In two separate terminals, run the following commands:
```bash
cargo run --bin serve --features=example -- --port 5355 --grpc-port 50051 127.0.0.1:50051 127.0.0.1:50052
```

```bash
cargo run --bin serve --features=example -- --port 5356 --grpc-port 50052 127.0.0.1:50051 127.0.0.1:50052
```

This will host two identical nodes, each listening on a different port. In practice, these would be container images hosted within the same cluster on different machines.

Each of these nodes can receive requests on their respective `port`s. The `grpc-port` is used for gRPC communication between the nodes. If any of them
are occupied with a request, it will forward the request to other nodes.

In separate terminals, run the following command twice:
```bash
curl -X POST -H "Content-Type: application/json" -d '{"body": "test"}' http://localhost:5355/send
```

By sending the same node two requests, you can see that the first request will be handled by the first node, and the second request will be forwarded to the second node, resulting in a different `worker` tag in the response:

```json
{"success": true,
 "worker": "worker://0.0.0.0:50051",
 "body": "Work done: test"}
```

```json
{"success": true,
 "worker": "worker://0.0.0.0:50052",
 "body": "Work done: test"}
```
