/** ***********************
 * Smart contract that pushes a SC containing a message handler
 * function and sends an asynchronous message to that same SC
 **/

import { send_message, print, create_sc, include_base64 } from "massa-sc-std"

export function main(name: string): void {
    const bytes = include_base64('./build/receive_message.wasm');
    const address = create_sc(bytes);
    send_message(address, "receive", 1, 1, 20, 1, 100_000, 1, 100, "hello my good friend!");
    print("receiver created and message sent")
}
