/** ****************************
 * Bytecode to send to the massa network that push the `helloworld`
 * smartcontract.
 *
 * N.B. The entry file of your Massa Smart Contract program should be named
 * `src/main.ts`, the command `yarn bundle` will produce an `build/main.wasm`
 * which is ready to be send on Massa network node!
 **/

import {Storage} from "@massalabs/massa-sc-std";

export function main(_args: string): void {
    Storage.set("TEST", "TEST_VALUE");
    Storage.set("TEST2", "TEST_VALUE2");
    Storage.del("TEST2");
}
