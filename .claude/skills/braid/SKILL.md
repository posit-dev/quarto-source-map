---
name: braid
description: braid issue tracking for this project. Use when filing, finding, updating, or closing work items ("strands"), or when the user mentions braid, issues, tasks, bugs, or what to work on.
---

<!-- BEGIN BRAID (managed by `braid agents-info --install`) -->
# braid issue tracking

This project tracks issues ("strands") with **braid**. For the
authoritative, version-matched usage guide — every command, flag, and
convention — run:

    braid agents-info

Core loop: `braid ready` finds workable strands; claim one with `braid
update <id> --status in_progress --assignee <you>`; leave a trail with
`braid comment <id> "..."`; finish with `braid close <id> --reason "..."`.
File discovered work as you go in one shot:

    braid create "<title>" --type <task|bug|...> --deps discovered-from:<current-id>

Attribute your changes with `BRAID_AUTHOR=<you>`.
<!-- END BRAID -->
