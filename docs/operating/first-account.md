The first account
=================

A new instance gets its first account the way a Mastodon server does, from the
command line rather than by signing up. Migrations seed Mastodon's Moderator,
Admin and Owner roles, and `eunha accounts create` is `tootctl accounts create`:

~~~~ sh
eunha accounts create gardener --email gardener@example.com \
  --confirmed --approve --role Owner
~~~~

It prints the random password the account was given. Sign-ups need not be open,
and no mail is sent, so it works before an instance has any mail provider. A
process serving a tenants directory names the instance:

~~~~ sh
eunha --tenants /path/to/tenants accounts create gardener \
  --instance garden.eunha.space --email gardener@example.com \
  --confirmed --approve --role Owner
~~~~

`eunha accounts modify gardener --reset-password` prints a new random password
and signs the account out of every session and app, as
`tootctl accounts modify --reset-password` does.
