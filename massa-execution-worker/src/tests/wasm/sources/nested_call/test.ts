/** ***********************
 * Smart contract containing a message handler function
 **/

import { print, generate_event, call } from "massa-sc-std"

export function receive(data: string): void {
    generate_event("receive:" + data);
    print(data);
}

export function test(address: string): void {
    print("in function test");
    call(address, "receive", "massa", 0);
}