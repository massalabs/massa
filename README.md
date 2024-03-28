<br />

<p align="center">
<img src="logo.png" width="240">
</p>

<br />

# Massa: The Decentralized and Scaled Blockchain

[![CI](https://github.com/massalabs/massa/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/massalabs/massa/actions/workflows/ci.yml?query=branch%3Amain)
[![Docs](https://img.shields.io/static/v1?label=docs&message=massa&color=&style=flat)](https://massalabs.github.io/massa/massa_node/)
[![Open in Gitpod](https://shields.io/badge/Gitpod-contribute-brightgreen?logo=gitpod&style=flat)](https://gitpod.io/#https://github.com/massalabs/massa)
[![codecov](https://codecov.io/gh/massalabs/massa/graph/badge.svg?token=598URC32TV)](https://codecov.io/gh/massalabs/massa)

## About Massa

[Massa](https://massa.net) is a new blockchain based on a [multithreaded technology](https://arxiv.org/pdf/1803.09029)
that supports more than 10'000 transactions per second in a fully decentralized network with thousands of nodes. A short
introduction video is available [here](https://www.youtube.com/watch?v=NUUFhvd7ulY).

Massa's purpose is to make it easy to deploy fully decentralized applications. We've developed two key technologies that
are exclusive to Massa to help make this possible:
[autonomous smart contracts](https://docs.massa.net/en/latest/general-doc/autonomous-sc.html) and native
[front-end hosting](https://docs.massa.net/en/latest/general-doc/decentralized-web.html).

Here is a list of tools to easily build applications on the Massa blockchain:

- [JS Client library](https://github.com/massalabs/massa-web3) to connect to the Massa blockchain from your applications.
- [AssemblyScript](https://github.com/massalabs/massa-as-sdk) SDKs to write smart contracts.
- [Examples of applications](https://github.com/massalabs/massa-sc-examples) built on Massa.
- [Explorer](https://test.massa.net).
- [Interactive API specification](https://playground.open-rpc.org/?schemaUrl=https://test.massa.net/api/v2&uiSchema\[appBar\]\[ui:input\]=false&uiSchema\[appBar\]\[ui:inputPlaceholder\]=Enter+Massa+JSON-RPC+server+URL&uiSchema\[appBar\]\[ui:logoUrl\]=https://massa.net/favicons/favicon.ico&uiSchema\[appBar\]\[ui:splitView\]=false&uiSchema\[appBar\]\[ui:darkMode\]=false&uiSchema\[appBar\]\[ui:title\]=Massa&uiSchema\[appBar\]\[ui:examplesDropdown\]=false&uiSchema\[methods\]\[ui:defaultExpanded\]=false&uiSchema\[methods\]\[ui:methodPlugins\]=true&uiSchema\[params\]\[ui:defaultExpanded\]=false).
- [Lots of documentation](https://docs.massa.net), from [web3 development](https://docs.massa.net/docs/build/home)
  to [Massa's architecture](https://docs.massa.net/docs/learn/home).

## Become a node runner

With decentralization as a core value, we've gone to great lengths to lower the barrier of entry for community
participation in our mainnet, and we [invite you to join in](https://docs.massa.net/docs/node/home) by
becoming a node runner!

## Community

We have active community discussions on several platforms, and we invite you to join the conversations. If you wish to
go into some depth about technical aspects, speak with people working in the ecosystem full-time, or just have a chat
with other like-minded people, these can be good places to start:

- [Discord](https://discord.com/invite/massa)
- [Telegram](https://t.me/massanetwork)
- [Twitter](https://twitter.com/MassaLabs)

## Contributing

We welcome contributions from the community at large, from anywhere between seasoned OSS contributors, to those hoping
to try it for the first time.

If you would like some help to get started, reach out to us on our community [Discord](https://discord.com/invite/massa)
server. If you're comfortable enough to get started on you're own, check out our
[good first issue](https://github.com/massalabs/massa/labels/good%20first%20issue) label.

### Contributors

A list of all the contributors can be found [here](CONTRIBUTORS.md)

## Community Charter

The Massa Community Charter is designed to protect decentralization.
You can find it here: [COMMUNITY_CHARTER.md](COMMUNITY_CHARTER.md)

## Initial distribution files

The following initial distribution files:
* `massa-node/base_config/initial_ledger.json`
* `massa-node/base_config/deferred_credits.json`
* `massa-node/base_config/initial_rolls.json`

Are copied from `https://github.com/Massa-Foundation/genesis-ledger/tree/main/node_initial_setup` at commit hash `9bb16c286d2bdc830490bd0af70571207d34921c`.
