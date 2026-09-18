Differential testing against a live Mastodon
============================================

The entity check compares eunha to what upstream's serializers *say*. This one
asks upstream directly: the same request goes to both servers and the responses
are compared.

~~~~
scripts/differential_test.sh                              # its own eunha
scripts/differential_test.sh http://localhost:3001 TOKEN  # one you run
~~~~

Given no arguments it brings up an eunha of its own — scratch database,
migrations, two accounts and a token — and tears it down afterwards. That form
exists so CI and a developer run the same path: the eunha-side setup used to
live in whoever had last run it, which is most of why this went weeks without
being run at all. It needs `target/release/eunha` built, as
`federation_test.sh` does.

That brings up Mastodon in Docker — the official image, because building it from
source on macOS means libidn, OpenSSL headers for `hiredis-client`, libvips, and
a `pg` gem that segfaults against Postgres 18, all of which the image has
already solved — mints tokens, and compares what a client actually does: 31
reads, nine writes, and the interaction verbs (favourite, boost, bookmark, pin,
follow, block, mute, and their undos).

The stack runs Sidekiq as well as the web process. Without a worker nothing
Mastodon defers ever happens, and some of that shows in the API — a home feed
stays `regenerating?` and answers 206 forever — which reads as a difference from
eunha when it is a missing worker. An unfaithful reference invents findings.

It invented ten. Sidekiq boots as soon as Postgres answers, but the schema is
created by the web process's `db:prepare`, so on a cold database the worker
started first, died on `relation "users" does not exist`, and compose did not
bring it back. `unfavourite` and `unreblog` hand the removal to a worker and
force the flag false in *their own* response, so each undo looked right while
the row survived — and every later request read that row and reported
`favourited: true` on a status that had just been unfavourited. Nine
`favourited` differences and one `reblogged`, all recorded against eunha, all of
them a dead worker. The worker now restarts until the schema exists, and the
harness asks Redis whether one has registered before it compares anything,
rather than trusting that a container was started.

Each do/undo pair also acts on a status of its own now. Sharing one across all
ten verbs meant one pair's deferred work was visible to the next, and `unreblog`
opens a window Mastodon disagrees with itself in: `Status.reblogs_map` is
`unscoped` and counts the discarded reblog, while `Account#reblogged?` goes
through `default_scope { recent.kept }` and does not, so two endpoints answer
differently about the same status depending on whether the controller passes a
relationships presenter. A pair per status leaves nothing to leak.

Nothing in it encodes what the answer should be, which is the point: a rule
misread while writing a test would be misread in the test too. It found seven
differences the source-reading had missed, all of them fields nested inside
objects the entity extraction never descended into.

It compares values on writes and interactions, where both servers act on the
same input so a count or a flag that differs is a real difference — that is
where `poll.voted` was `false` for a poll's own author, `noindex` told every
account to hide from search engines, and a status came back with no language at
all. Identifiers, hostnames, timestamps and totals over an instance's whole
history are excluded, because two servers cannot agree on those however
identical the request, and comparing them buries everything else.

For reads it compares values only under `configuration.*` — the limits an
instance states about itself, where two servers genuinely should agree. That
came second, after a shape-only comparison passed `max_display_name_length: 30`
against Mastodon's 40, both being integers. Comparing values found the media
description limit still advertised at Mastodon's older 1500 rather than 10,000.

Everything else stays shape-only on purpose. `followers_count`, ids and
timestamps depend on each instance's data, and comparing them would bury real
findings under differences that mean nothing.

It runs in CI, as its own job, for the reason everything else here does: the two
ways it broke — a reference with no worker, and a compose file another harness
had edited out from under it — were both invisible to anyone not running it.

One comparison is built rather than observed. **Notification grouping** is the
part of that API which is not a straight translation of a row: Mastodon
collapses notifications into groups, and a client renders “X and 2 others
favourited your post” out of a group's `notifications_count` and
`sample_account_ids`. A server that groups differently shows a different
sentence with every field present and of the right type, so shape cannot see it
— and neither can one account, because a group of one is a group on any server.
Three further accounts favourite the same status and follow the same account,
and the groups are compared. Account ids cannot match between two servers, so
the samples are compared by *who* they name: each fan is known by the position
it acts in, and a group naming `[fan3, fan2, fan1]` here has to name
`[fan3, fan2, fan1]` there. It agrees, on both the count and the order.

Getting there needed the fixture reset on both sides, and the second reset is
the one worth remembering: eunha gets a scratch database every run while the
Mastodon container is left up between them, so clearing the notifications is not
enough. A repeat follow produces no notification at all, so on a second run only
the server with a fresh database reports a follow group — which reads exactly
like eunha inventing one. The fans unfollow before they follow.

The groups are also read only once every act has become a notification.
Mastodon writes them from `LocalNotificationWorker`, after the favourite or
follow has already answered, while eunha writes them in the request — so reading
at once caught Mastodon with the third fan's follow still queued, a group of two
against eunha's three, recorded as eunha's difference.
