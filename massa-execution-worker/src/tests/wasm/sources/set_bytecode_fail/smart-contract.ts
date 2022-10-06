/** ***********************
 * Smart contract exporting a public function `helloworld`
 *
 * N.B. The entry file of your AssemblyScript program should be named
 * `src/smart-contract.ts`, the command `yarn build` will produce an
 * `build/smart-contract.wasm` WASM binary!
 **/

export function helloworld(name: string): string {
    return `Hello, ${name}!`;
}
