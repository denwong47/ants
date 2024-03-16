# Worker

A worker that can do work or distribute work.

A unit of work is defined by an async function that takes a type `T` and returns
a `Result<R, E>`. All workers are expected to be stateless, and work performed
by any worker is expected to be the same as any other worker.

The work is also expected to be blocking for the duration of the work, and
no concurrent work can be performed. This is usually the case when the work
involves physical devices such as GPU, or the work being extremely CPU
bound, rendering concurrent work to be ineffective.

The worker should be instantiated with a list of nodes that it can forward
work to, alongside the aforementioned function. The worker will then listen
on gRPC for work requests. Any calls to `Worker::find_worker_and_work` will
attempt to reserve the worker, and do work if possible, or forward the work
to another worker if not.

During the negotiation of work with another worker, the worker will attempt
to reserve the other worker before forwarding the work. If the other worker
is reserved, the worker will attempt to reserve the next worker in the list.

Reservation works by sending a `reserve` request to the worker, and the
worker will respond with a token if it is not reserved. A subsequent `work`
request must include the token to be accepted; by the end of the work, the
worker will release the reservation. If the worker does not receive a
`work` request within a [`token::TIMEOUT`], the worker will release the
reservation automatically.
