/** ****************************
 * Bytecode to send to the massa network that push the `helloworld`
 * smartcontract.
 *
 * N.B. The entry file of your Massa Smart Contract program should be named
 * `src/main.ts`, the command `yarn bundle` will produce an `build/main.wasm`
 * which is ready to be send on Massa network node!
 **/

import {createSC, fileToBase64, generateEvent, print} from "@massalabs/massa-sc-std";

export function main(_args: string): void {
    const bytes = fileToBase64('./build/test.wasm');
    const address = createSC(bytes);
    generateEvent(address.toByteString());
    print("main:" + address.toByteString());
}
