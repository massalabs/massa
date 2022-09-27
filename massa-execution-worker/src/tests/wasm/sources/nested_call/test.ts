/** ***********************
 * Smart contract containing a message handler function
 **/

 import { print, generateEvent, call, Context, Address } from "@massalabs/massa-sc-std"

 export function receive(data: string): void {
     print("Gas at start inner: " + Context.remainingGas().toString());
     generateEvent(Context.remainingGas().toString());
     print(data);
     print("Gas at end inner: " + Context.remainingGas().toString());
     generateEvent(Context.remainingGas().toString());
 }
 
 export function test(address: string): void {
     print("Gas at start: " + Context.remainingGas().toString());
     generateEvent(Context.remainingGas().toString());
     call(Address.fromByteString(address), "receive", "massa", 0);
     print("Gas at end: " + Context.remainingGas().toString());
     generateEvent(Context.remainingGas().toString());
 }