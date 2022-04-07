/** ****************************
 * Bytecode to send to the massa network that push the `helloworld`
 * smartcontract.
 **/

import {print, create_sc, include_base64, generate_event } from "massa-sc-std"

export function main(name: string): void {
    const bytes = include_base64('./build/test.wasm');
    const address = create_sc(bytes);
    generate_event(address);
    print("main:" + address);
}