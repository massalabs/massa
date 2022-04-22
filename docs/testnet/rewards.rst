.. _testnet-rewards:

===============================
Testnet Staking Rewards Program
===============================

To help achieve our goal of a fully decentralized and scaled blockchain,
we designed a staking rewards program during the testnet phase.

People that consistently run a node and produce blocks will be rewarded
mainnet Massa tokens when mainnet launches.

Staking is what improves the security of the network. By buying rolls
(freezing your coins) and producing your share of the blocks, you help
honest nodes collectively protect against potential attackers, who must
not reach 51% of the block production. On mainnet, staking is
incentivized through block rewards: for each block produced, you get
some Massa. On testnet however, you get testnet Massa which has no
value, this is why we will reward you with mainnet Massa for learning to
set up your node and stake right now, which also helps us improve the
staking user experience.

On July 16th we launched the first public version of Massa, the first
testnet. More than 350 nodes were connected at the same time after one
week, which overloaded our bootstrap nodes which were the only nodes
accepting connections. By setting your node up to be routable (with a
public IP), you become a real peer in the peer-to-peer network: you not
only connect to existing routable nodes, but you offer other people the
possibility to access the network through your connection. We will
therefore also reward how often your node is publicly accessible.

Episodes
========

We have release cycles of 1 month each, called "episodes", the
first one started in August 2021. At the beginning of an episode,
participants have a few days to set up their nodes with the newest
version before scoring starts, but it's also possible to join anytime
during the episode.

Throughout the episode, you can ask for coins in the Discord faucet (on
channel `testnet-faucet`). No need to abuse the faucet, we don't
reward you based on the number of rolls.

At the end of an episode, all nodes stop by themselves and become
useless/unusable. Participants have to download and launch the new
version for the next episode. Make sure you keep the same node private
key and wallet!

Scoring Formula
---------------

The score of a node for a given episode is the following:

    Score = 50 * (active_cycles / nb_cycles) * (produced_blocks / selected_slots) + 50 * (routable_cycles / nb_cycles) + 20 * total_maxim_factor / nb_cycles

-   50 points of the score are based on staking:

    -   (`active_cycles` / `nb_cycles` ) \* (`produced_blocks` /
        `selected_slots`)

        -   `active_cycles` is the number of cycles in the episode
            during which the address had active rolls.
        -   `nb_cycles` is the total number of cycles in the episode.
        -   `produced_blocks` is the number of final blocks produced by
            the staker during the episode.
        -   `selected_slots` is the number of final slots for which the
            staker was selected to create blocks during the episode. If
            `selected_slots` = 0, the staking score is set to 0.
        -   The maximum score is supposed to be reached if, during the
            whole episode, the node has rolls and produces all blocks
            when it is selected to.
-   50 points of the score are based on the routability of the node: how
    often the node can be reached by other nodes.

    -   `routable_cycles` / `nb_cycles`

        -   `routable_cycles` is the number of connection trials that
            resulted in a successful connection.
        -   Maximum score is achieved if the node can always be reached
            by other nodes.
-   20 points of the score incentivize node diversity: the network is
    more decentralized if nodes are spread across countries and
    providers than if they are all hosted at the same location/provider.

    -   `total_maxim_factor` / `nb_cycles`

        -   `total_maxim_factor` is the total amount of `maxim_factor`
            accumulated at each cycle. The `maxim_factor` is a value
            between 0 and 1 representing the distance between this
            node's IP address and the IP addresses of other nodes in a
            given cycle.
        -   Maximum score is reached when running the node at home or
            with a provider that is not used to run other Massa nodes.

We encourage every person to run only one node. Running multiple nodes
with the same staking keys will result in roll slashing in the future.
Running multiple nodes with the same node_privkey.key also reduces
network health and will be a point of attention for rewards.

Registration
------------

To validate your participation in the testnet staking reward program,
you have to register with your Discord account. Write something in the
`testnet-rewards-registration` channel of our
`Discord <https://discord.com/invite/massa>`_ and our bot will DM you
instructions.

From scores to rewards
----------------------

The launch of mainnet is planned for mid-2022.

By this time, people will be able to claim their rewards through a KYC
process (most likely including video/liveness) to ensure that the same
people don't do multiple claims, and comply with KYC/AML laws.

The testnet score of a person will be the sum of all their episode
scores.

The mainnet reward will depend on the testnet score. More info on
mainnet rewards will come later.
