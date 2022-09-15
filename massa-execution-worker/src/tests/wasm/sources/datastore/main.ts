import { print, generateEvent, getOpKeys } from "@massalabs/massa-sc-std";

export function main(_args: string): void {

    let keys: Array<StaticArray<u8>> = getOpKeys();
    // print(`keys: ${keys}`);
    let msg = `keys: ${keys}`;
    generateEvent(msg);
}

