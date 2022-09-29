import { print, generateEvent, hasOpKey, getOpData, getOpKeys } from "@massalabs/massa-as-sdk";

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

    print("Get op data...");

    let key3: StaticArray<u8> = [9];
    let data_key_1: StaticArray<u8> = getOpData(key1);
    let data_key_3: StaticArray<u8> = getOpData(key3);

    generateEvent(`data key 1: ${data_key_1} - data key 3: ${data_key_3}`);
    // generateEvent(`data key 1: ${data_key_1}`);
    // generateEvent(`data key 3: ${data_key_3}`);

}

