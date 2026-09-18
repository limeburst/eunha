Invites
=======

Who may invite is Mastodon's `invite_users` permission, and it lives on the
**everyone role** — `user_roles` id -99, seeded by migration 009 with
`UserRole::Flags::DEFAULT`, which is that one permission. Every member has that
role unless given another, so an instance invites the way upstream does until it
says otherwise. One that would rather hand invites out itself takes the bit off:

~~~~
UPDATE user_roles SET permissions = permissions & ~(1 << 16) WHERE id = -99;
~~~~

Staff keep it through their own role. A role carrying `administrator` (1 << 0)
computes to every permission there is, and any other role's permissions are
unioned with the everyone role's — Mastodon's `UserRole#computed_permissions`,
which is why upstream's Admin role does not list `invite_users` and an admin can
still invite. `verify_credentials` reports that computed set rather than the raw
column, so a client hides the invite page for exactly the accounts the server
would refuse.

Eunha has no role editor and no `eunha` subcommand for one: roles are rows, and
an instance that needs to change one runs the `UPDATE`. Upstream will only let
the everyone role hold `Flags::SAFE` — `invite_users` and
`invite_bypass_approval` — and nothing here enforces that, so a hand-written
value should stay inside it.

The other half of `SAFE` is what an invite does to the approval queue. On an
instance with `approval_required`, an invite skips review only when whoever
wrote it holds `invite_bypass_approval`, which is Mastodon's
`Invite#bypass_approval?` — a question about the inviter, not about whether an
invite was used. The everyone role does not carry it, so an ordinary member may
bring someone and the admin still sees them first. An instance that would rather
an invite be the whole of the decision grants it:

~~~~
UPDATE user_roles SET permissions = permissions | (1 << 21) WHERE id = -99;
~~~~

Staff invites bypass already, through the `administrator` flag.


Handing them out
----------------

That leaves an instance where only staff may invite, which on its own would
flatten the invite tree: every arrival would be a child of the admin rather than
of whoever actually brought them. So an admin can mint codes **into a member's
own account** instead — `POST /api/eunha/v1/invite_grants`, and the “Hand out
invites” panel on the invite page, for one member or for the whole userbase at
once. They appear on that member's page to copy and pass on, and a signup
through one lands under them.

The count is the limit; there is no allowance to keep books on, because the
codes themselves are the allowance. `manage_invites` is what it takes to hand
them out, and listing your own invites takes no permission at all — a member who
cannot create one still has to be able to read what they were given, which is
where eunha parts company with `InvitesController#index`.
