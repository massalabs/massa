===========
Routability
===========

Principle
=========

Nodes in the Massa network need to establish connections between them to
communicate, propagate blocks and operations, and maintain consensus and
synchrony all together.

For node A to establish a connection towards node B, node B must be
routable. This means that node B has a public IP address that can be
reached from node A and that ports TCP 31244 and TCP 31245 are open on
node B and that inbound connection on those ports are allowed by
firewalls on node B. Once such a connection is established,
communication through this connection is bidirectional, and it does not
matter anymore which one of the two nodes initiated the connection
establishment.

If only a small number of nodes are routable, all other nodes will be
able to connect only to those routable nodes, which can overload them
and generally hurt the decentralization and security of the network, as
those few routable nodes become de-facto central communication hubs,
choke points, and single points of failure. It is therefore important to
have as many routable nodes as possible.

In Massa, nodes are non-routable by default and require a manual
operation to be made routable.

How to make your node routable
==============================

-   make sure the computer on which the node is running has a static
    public IP address (IPv4 or IPv6). You can retrieve the public IP
    address of your computer by opening <https://api.ipify.org>
-   if the computer running the node is behind a router/NAT, you will
    need to configure your router:

    -   if the router uses DHCP, the MAC address of the computer running the
        node must be set to have a permanent DHCP lease (a local IP address
        that never changes, usually of form 192.168.X.XX)
    -   incoming connections on TCP ports 31244 and 31245 must be directed
        towards the local IP address of the computer running the node
-   setup the firewall on your computer to allow incoming TCP
    connections on ports 31244 and 31245 (example:
    :code:`ufw allow 31244 && ufw allow 31245` on Ubuntu, or set up the
    Windows Firewall on Windows)
-   edit file :code:`massa-node/config/config.toml` (create it if absent) with the following
    contents:

    .. code-block:: toml

        [network]
        routable_ip = "AAA.BBB.CCC.DDD"

    where AAA.BBB.CCC.DDD should be replaced with your public IP address (not
    the local one !). IPV6 is also supported.
-   run the massa node
-   you can then test if your ports are open by typing your public IP
    address and port 31244 in
    https://www.yougetsignal.com/tools/open-ports/ (then again with
    port 31245)
-   Once your node is routable, you need to send the public IP address of your node to the Discord bot.
    You first need to register to the staking reward program (see the last step below).

Last step
=========

-   To validate your participation in the testnet staking reward program,
    you have to register with your Discord account. Write something in the
    `testnet-rewards-registration` channel of our
    `Discord <https://discord.com/invite/massa>`_ and our bot will DM you
    instructions. More info here: `Testnet rewards program <https://github.com/massalabs/massa/wiki/testnet_rules>`_
