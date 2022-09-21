/** ***********************
 * Smart contract testing all the operation datastore functions
 **/
import { print, generateEvent, getOpKeys, hasOpKey } from "@massalabs/massa-sc-std";

export function main(_args: string): void {

    let keys: Array<StaticArray<u8>> = getOpKeys();
    // print(`keys: ${keys}`);
    let msg = `keys: ${keys}`;
    generateEvent(msg);

    let key1: StaticArray<u8> = [65, 66];
    let key2: StaticArray<u8> = [0, 0, 1, 1, 11, 11, 99];
    let has_key_1: bool = hasOpKey(key1);
    let has_key_2: bool = hasOpKey(key2);
    msg = `has_key_1: ${has_key_1} - has_key_2: ${has_key_2}`
    // print(msg);
    generateEvent(msg);

}

