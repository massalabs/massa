import { generate_event, call } from "massa-sc-std";

export function main(_args: string): void {
    call("invalid_addr", "invalid_func", "invalid_param", 42);
    generate_event("Hello world!");
}
