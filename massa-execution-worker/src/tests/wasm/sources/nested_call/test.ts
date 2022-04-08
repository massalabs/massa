/** ***********************
 * Smart contract containing a message handler function
 **/

import { print, generate_event, call, Context } from "massa-sc-std"

export function receive(data: string): void {
    print("Gas at start inner: " + Context.get_remaining_gas().toString());
    generate_event(Context.get_remaining_gas().toString());
    print(data);
    print("Gas at end inner: " + Context.get_remaining_gas().toString());
    generate_event(Context.get_remaining_gas().toString());
}

export function test(address: string): void {
    print("Gas at start: " + Context.get_remaining_gas().toString());
    generate_event(Context.get_remaining_gas().toString());
    call(address, "receive", "massa", 0);
    print("Gas at end: " + Context.get_remaining_gas().toString());
    generate_event(Context.get_remaining_gas().toString());
}