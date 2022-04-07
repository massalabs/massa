/** ***********************
 * Smart contract containing a message handler function
 **/

import { print, generate_event } from "massa-sc-std"

export function receive(data: string): void {
    let response: string = "message received: " + data;
    generate_event(response);
    print(response);
}
