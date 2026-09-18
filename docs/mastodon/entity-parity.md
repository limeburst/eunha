API entity parity
=================

The schema check answers whether eunha's database matches Mastodon's. The
entity check answers the API half: a client reads fields by name, so a missing
one breaks it and an unexpected one is a divergence nobody decided on.

`mastodon/entities.json` records what each of Mastodon's REST entities carries,
and tests fetch real responses from a running eunha and compare. It is built
from two sources because neither is sufficient alone — `app/serializers/rest`
decides what is actually emitted, and `app/javascript/mastodon/api_types` states
plainly which fields are optional, where the serializers hide that behind `if:`
conditions. 4.7.1's instance serializer emits `icon` and `wrapstodon` that the
TypeScript does not mention, so the serializers are the authority on what
exists.

~~~~
mise run entities:build        # re-record from a Mastodon checkout
~~~~

That reads a clone at `~/Git/mastodon` (`MASTODON_REPO` to point elsewhere) at
the tracked tag rather than its working tree, fetching tags if the tag is
missing. Mastodon is not a submodule: 424MB of history for 468KB of files that
only matter when adopting a release, and a submodule bump's diff is a SHA,
whereas the diff of what is recorded here *is* the change being adopted.
