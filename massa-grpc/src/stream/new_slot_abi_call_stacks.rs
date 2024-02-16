use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1::{self as grpc_api, AscabiCallStack, OperationAbiCallStack};
use std::pin::Pin;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

use crate::error::match_for_io_error;
use crate::{error::GrpcError, public::into_element, server::MassaPublicGrpc};

/// Type declaration for NewSlotExecutionOutputs
pub type NewSlotABICallStacksStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewSlotAbiCallStacksResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// Creates a new stream of new slots abi call stacks
pub(crate) async fn new_slot_abi_call_stacks(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewSlotAbiCallStacksRequest>>,
) -> Result<NewSlotABICallStacksStreamType, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Extract the incoming stream of abi call stacks messages
    let mut in_stream = request.into_inner();

    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_traces_sender
        .subscribe();
    // let grpc_config = grpc.grpc_config.clone();

    println!("new_slot_abi_call_stacks...");

    tokio::spawn(async move {
        loop {
            select! {
                // Receive a new slot execution traces from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_slot_execution_trace) => {

                            if massa_slot_execution_trace.asc_call_stacks.is_empty() &&
                                massa_slot_execution_trace.operation_call_stacks.is_empty() {
                                continue;
                            }

                            let mut ret = grpc_api::NewSlotAbiCallStacksResponse {
                                slot: Some(massa_slot_execution_trace.slot.into()),
                                asc_call_stacks: vec![],
                                operation_call_stacks: vec![]
                            };

                            for (i, asc_call_stack) in massa_slot_execution_trace.asc_call_stacks.iter().enumerate() {
                                ret.asc_call_stacks.push(
                                    AscabiCallStack {
                                        index: i as u64,
                                        call_stack: asc_call_stack.iter().map(|t| into_element(t)).collect()
                                    }
                                )
                            }
                            for (op_id, op_call_stack) in massa_slot_execution_trace.operation_call_stacks.iter() {
                                ret.operation_call_stacks.push(
                                    OperationAbiCallStack {
                                        operation_id: op_id.to_string(),
                                        call_stack: op_call_stack.iter().map(|t| into_element(t)).collect()
                                    }
                                )
                            }

                            if let Err(e) = tx.send(Ok(ret)).await {
                                error!("failed to send new slot execution trace: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("error on receive new slot execution trace : {}", e)
                        }
                    }
                }
                // Receive a new message from the in_stream
                res = in_stream.next() => {
                    match res {
                        Some(res) => {

                            match res {
                                Ok(message) => warn!("Received {:?}", message),
                                Err(e) => {
                                    // Any io error -> break
                                    if let Some(io_err) = match_for_io_error(&e) {
                                        warn!("client disconnected, broken pipe: {}", io_err);
                                        break;
                                    }
                                    error!("{}", e);
                                    if let Err(e2) = tx.send(Err(e)).await {
                                        error!("failed to send back error response: {}", e2);
                                        break;
                                    }
                                }
                            }
                        }
                        None => {
                            // the client has disconnected
                            break;
                        }
                    }
                }
            }
        }
    });

    // Spawn a new Tokio task to handle the stream processing
    /*    tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(500));

            // Continuously loop until the stream ends or an error occurs
            loop {
                select! {
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(Ok(_req)) => {
                                // TODO
                            },
                            _ => {
                                // Client disconnected
                                break;
                            }
                        }
                    },
                    _ = interval.tick() => {
                        if let Err(e) = tx
                            .send(Ok(grpc_api::NewSlotAbiCallStacksResponse {
                                slot: Some(Slot::new(0, 0).into()),
                                asc_call_stacks: vec![
                        AscabiCallStack {
                            index: 0,
                            call_stack: vec![
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins"),
                                        parameters: vec![String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins_for"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins_for"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("get_balance"),
                                        parameters: vec![],
                                        return_value: String::from("10")
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("set_data"),
                                        parameters: vec![String::from("73616c75746a656d617070656c6c65617572656c69656e"), String::from("73616c75746a656d617070656c6c65617572656c69656e")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::ElementCall(AbiCallStackElementCall {
                                        name: String::from("call"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("best_function"), String::from("73616c75746a656d617070656c6c65617572656c69656e"), String::from("10")],
                                        return_value: String::from("73616c75746a656d617070656c6c65617572656c69656e"),
                                        sub_calls: vec![
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins"),
                                                    parameters: vec![String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins_for"),
                                                    parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins_for"),
                                                    parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("get_balance"),
                                                    parameters: vec![],
                                                    return_value: String::from("10")
                                                }))
                                            },
                                        ]
                                    }))
                                }
                            ]
                        },
                        AscabiCallStack {
                            index: 1,
                            call_stack: vec![
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins"),
                                        parameters: vec![String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                            ]
                        }
                    ],
                    operation_call_stacks: vec![
                        OperationAbiCallStack {
                            operation_id: String::from("O12mh1zn9oDr9aUEp4YzPbiShDtyJW1gFUB1d4riYR5wSCqPGuQ"),
                            call_stack: vec![
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins"),
                                        parameters: vec![String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins_for"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("transfer_coins_for"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("get_balance"),
                                        parameters: vec![],
                                        return_value: String::from("10")
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                        name: String::from("set_data"),
                                        parameters: vec![String::from("73616c75746a656d617070656c6c65617572656c69656e"), String::from("73616c75746a656d617070656c6c65617572656c69656e")],
                                        return_value: String::new()
                                    }))
                                },
                                AbiCallStackElementParent {
                                    call_stack_element: Some(CallStackElement::ElementCall(AbiCallStackElementCall {
                                        name: String::from("call"),
                                        parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("best_function"), String::from("73616c75746a656d617070656c6c65617572656c69656e"), String::from("10")],
                                        return_value: String::from("73616c75746a656d617070656c6c65617572656c69656e"),
                                        sub_calls: vec![
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins"),
                                                    parameters: vec![String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins_for"),
                                                    parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("transfer_coins_for"),
                                                    parameters: vec![String::from("AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp"), String::from("AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW"), String::from("10")],
                                                    return_value: String::new()
                                                }))
                                            },
                                            AbiCallStackElementParent {
                                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                    name: String::from("get_balance"),
                                                    parameters: vec![],
                                                    return_value: String::from("10")
                                                }))
                                            },
                                        ]
                                    }))
                                }
                            ]
                        }
                    ]
                            }))
                            .await
                        {
                            // Log an error if sending the response fails
                            error!("failed to send back transactions_throughput response: {}", e);
                            break;
                        }
                    }
                }
            }
        });
    */
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewSlotABICallStacksStreamType)
}
