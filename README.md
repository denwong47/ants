## Ants

![Rust CI Badge](https://github.com/denwong47/ants/actions/workflows/rust-CI.yml/badge.svg?branch=main)

A naive implementation of distributed system to do arbitrary work.

In short, this is a cluster of API servers hosting identical image, each capable
of handling requests; but if a node is busy, it will forward the request to
another node. The list of nodes is stored as a min heap, prioritizing the node
that has not been called for the longest time. There is no leader in this
system; each node is equal.

### Motivation

The motivation for this project is for self-hosted LLMs which tends to occupy
a whole consumer GPU for a long time. They also tend to be memory intensive,
not to mention that their failure modes during concurrent calls are not
consistent - Out of memory, segfault, CUDA errors, etc. This project aims to
create a simple system that can balance the load between nodes, reducing the
chance of a node being overwhelmed into recovery mode, causing even more
downtime.

The architecture is also designed to be homelabs friendly - it is assumed that
each node already hosts some sort of IoT services, which will ping a loopback
first to keep things local. This is why multiple API endpoints are used
instead of a single entrypoint with a load balancer - which is a single point
of failure.

### Aspirations
- [X] Implement a distributed system that has no leaders; each node is equal.
- [X] Any of the nodes can receive a request, that if it can't handle,
      will forward to another node. Each time it tries to forward a request but
      failed, it will try itself first before trying the next node, since we
      assume that the node the user called is the closest to the user.
- [X] Communicate using gRPC protocol, with preset enum of message types.
- [X] The system should be able to handle node failures, and continue to work.
- [X] The list of nodes should be stored as a min heap using the last called time
      as the key. This way, the node that has been idle the longest will be the
      first to receive a request. This could be tweaked to take into account the
      distance between nodes; but currently all nodes are assumed to be
      clustered in the same subnet and have the same latency.
- [X] The nodes should discover each other using multicast, rather than
      a static list.
- [ ] The nodes should keep track of the correctness of each other, and remove a
      node from the list if it regularly fails to reserve, timeout or return
      corrupted results.
- [X] The system should be able to ask multiple nodes to do the same work, until
      some results agree with each other. This is to prevent a node from returning
      corrupted results.
- [ ] The above simple consensus is working, but it places a heavy bias on the host node
      that was called first. This is due to the current reservation system favouring
      the local node before handing work off to another node. The reservation
      system will need to be reworked to be more fair._
- [X] Integration and unit tests.

### Pre-requisites

The `protobuf` crate requires `protoc` to be installed. See [Protocol Buffer Compiler Installation](https://grpc.io/docs/protoc-installation/) for more information.

### Simple demo

In two separate terminals, run the following commands:
```bash
cargo run --bin serve --features=example -- --port 5355 --grpc-port 50051
```

```bash
cargo run --bin serve --features=example -- --port 5356 --grpc-port 50052
```

This will host two identical nodes, each listening on a different port. In
practice, these would be container images hosted within the same cluster on
different machines. In the following example, we will have 3 nodes setup.

Upon initiation, each of the node will send out multicasts to announce their presence,
to which any existing nodes will reply by the same. This will allow each of the node
to build up a list of nodes that they can relay work to.

Each of these nodes can receive requests on their respective `port`s. The
`grpc-port` is used for gRPC communication between the nodes. If any of them
are occupied with a request, it will forward the request to other nodes.

In Python, or whatever flavour of cURL you desire, send multiple concurrent requests
to any of the node asking for work to be done:

```python
from concurrent.futures import ThreadPoolExecutor
import requests

with ThreadPoolExecutor() as executor:
    responses = list(executor.map(
        lambda x: requests.post(
            "http://localhost:5355/send",
            json={"body": f"test {x}"}
        ).json(),
        range(5)
    ))
```

By sending the same node multiple requests, you can see that the first request will
be handled by the first node, and the second request will be forwarded to the
second node, resulting in a different `worker` tag in the response:

```python
[{'success': True,
  'worker': 'worker://0.0.0.0:50051',
  'body': 'Work done: test 0'},
 {'success': True,
  'worker': 'worker://0.0.0.0:50052',
  'body': 'Work done: test 1'},
 {'success': True,
  'worker': 'worker://0.0.0.0:50053',
  'body': 'Work done: test 2'},
 {'success': True,
  'worker': 'worker://0.0.0.0:50052',
  'body': 'Work done: test 3'},
 {'success': True,
  'worker': 'worker://0.0.0.0:50053',
  'body': 'Work done: test 4'}]
```

Since we have more work than we have nodes, some work will have to wait until some nodes
can be recycled. This is done transparently in the background - the only drawback is
longer latency for those calls.
