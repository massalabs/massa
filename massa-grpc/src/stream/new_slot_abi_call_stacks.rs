use futures_util::StreamExt;
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{
    self as grpc_api, abi_call_stack_element_parent::CallStackElement, AbiCallStackElement,
    AbiCallStackElementCall, AbiCallStackElementParent, AscabiCallStack, OperationAbiCallStack,
};
use std::{pin::Pin, time::Duration};
use tokio::{select, time};
use tonic::{Request, Streaming};
use tracing::error;

use crate::{error::GrpcError, server::MassaPublicGrpc};

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

    // Spawn a new Tokio task to handle the stream processing
    tokio::spawn(async move {
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
                                    parameters: vec![String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'number', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("transfer_coins_for"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("transfer_coins_for"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("get_balance"),
                                    parameters: vec![],
                                    return_value: String::from("{type: 'u64', value: 10}")
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("set_data"),
                                    parameters: vec![String::from("{type: 'byteArray', value: [1, 2, 3]}"), String::from("{type: 'byteArray', value: [4, 5, 6]}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::ElementCall(AbiCallStackElementCall {
                                    name: String::from("call"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'best_function'}"), String::from("{type: 'byteArray', value: [1, 2, 3]}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::from("{type: 'byteArray', value: [1, 2, 3]}"),
                                    sub_calls: vec![
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'number', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins_for"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins_for"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("get_balance"),
                                                parameters: vec![],
                                                return_value: String::from("{type: 'u64', value: 10}")
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
                                    parameters: vec![String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'number', value: 10}")],
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
                                    parameters: vec![String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'number', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("transfer_coins_for"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("transfer_coins_for"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("get_balance"),
                                    parameters: vec![],
                                    return_value: String::from("{type: 'u64', value: 10}")
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                    name: String::from("set_data"),
                                    parameters: vec![String::from("{type: 'byteArray', value: [1, 2, 3]}"), String::from("{type: 'byteArray', value: [4, 5, 6]}")],
                                    return_value: String::new()
                                }))
                            },
                            AbiCallStackElementParent {
                                call_stack_element: Some(CallStackElement::ElementCall(AbiCallStackElementCall {
                                    name: String::from("call"),
                                    parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'best_function'}"), String::from("{type: 'byteArray', value: [1, 2, 3]}"), String::from("{type: 'u64', value: 10}")],
                                    return_value: String::from("{type: 'byteArray', value: [1, 2, 3]}"),
                                    sub_calls: vec![
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'number', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins_for"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("transfer_coins_for"),
                                                parameters: vec![String::from("{type: 'string', value: 'AU12L4gaQ8j8j5yBt2jSmcsmu51yZW2gLjnZr5rAWnjKJDNacR3jp'}"), String::from("{type: 'string', value: 'AU12NT6c6oiYQhcXNAPRRqDudZGurJkFKcYNLPYSwYkMoEniHv8FW'}"), String::from("{type: 'u64', value: 10}")],
                                                return_value: String::new()
                                            }))
                                        },
                                        AbiCallStackElementParent {
                                            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                                                name: String::from("get_balance"),
                                                parameters: vec![],
                                                return_value: String::from("{type: 'u64', value: 10}")
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

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewSlotABICallStacksStreamType)
}
