Federating with a live Mastodon
===============================

The differential harness asks whether the two servers answer a client the same
way. This one asks whether they can talk to *each other*: eunha's other
federation tests run eunha against eunha, where both sides share eunha's reading
of ActivityPub, so a misreading is invisible. This builds a pair that shares
nothing but the specification.

~~~~
scripts/federation_test.sh
scripts/federation_test.sh --keep      # leave both up to poke at
~~~~

Twenty-four checks, both directions: resolve the account, follow, deliver a
status, favourite it, boost it, delete it, and see each land on the other side.

**Both servers run inside the container network.** eunha used to run on the host
behind a host-side Caddy, and that cannot work: `mastodon.test` is a network
alias, so a host process cannot resolve it — eunha could not fetch an actor's
public key and rejected every inbound activity with a 401. In the network they
resolve each other by name, and the harness needs no `/etc/hosts` entry, no
certificate trusted on the host, and no eunha built for this machine. The
certificates are a throwaway CA made with `openssl`, trusted by both sides
through `SSL_CERT_FILE` and by nothing else; the script drives both servers over
plain published ports, which is why nothing on the host has to trust them.

Two things that cost a while, both recorded in the script:

 -  **Rails refuses a request whose `Host` is not its `LOCAL_DOMAIN`** — 403 on
    every endpoint, including those needing no authentication. Since
    `mastodon.test` does not resolve on the host, the only way in is the
    published port with the name supplied by hand. Without it the readiness
    loop spins forever against a Mastodon that is up and answering.
 -  **A follow commits at different moments on the two sides.** The sender
    listing the receiver as a follower is what makes it *deliver*; the receiver
    having committed the follow is what makes it *keep* what arrives, because
    Mastodon drops an activity from an account no local account follows yet.
    Waiting on the sender alone leaves a gap in which a status is delivered,
    accepted with a 2xx, and silently discarded — one run missed by 2.9ms, and
    it read as eunha failing to deliver.

The run also exercises what nothing else did: eunha delivers to Mastodon's
**shared inbox** (`https://mastodon.test/inbox`), not its per-actor one.
Mastodon delivers to eunha's per-actor inbox, which is its own choice when there
is a single recipient on the far side.
