# Core texts (urfin-core, crate::text::tr).
core-unknown-command = Unknown command: { $id }
core-command-failed = { $id }: { $error }
core-task-panicked = Task { $task } failed unexpectedly
core-not-found = Not found: { $name }
core-no-match = Nothing matches { $mask }
core-no-bookmark = Bookmark { $n } is empty
core-last-tab = Cannot close the last tab
core-no-tab = No tab { $n }
core-config-warnings = Settings file has problems ({ $count }): { $first }
core-crashed = Urfin closed unexpectedly last time. Details: { $file }
core-action-show-logs = Show logs
core-action-dismiss = Dismiss
vfs-job-dir-size = Directory sizes
vfs-dir-size-none = No folders to measure: put the cursor on a folder or select folders
ops-not-yet = Not available yet: { $what }
ops-nothing = Nothing to do: put the cursor on a file or select files
ops-dest-empty = Destination is empty
ops-dest-file = Destination is a file: { $path }
ops-dest-mkdir-failed = Cannot create { $path }: { $error }
ops-conflict-bad = Unknown conflict policy: { $what }
ops-filter-bad = Bad file mask: { $mask }
ops-job-copy = { $n ->
    [one] Copy { $name }
   *[other] Copy { $n } items
}
ops-job-move = { $n ->
    [one] Move { $name }
   *[other] Move { $n } items
}
ops-copy-same = Cannot copy a file onto itself
ops-copy-into-self = Cannot copy a folder into itself
ops-copy-type = A file and a folder have the same name
ops-copy-no-name = No free name for a copy
ops-move-src-left = Copied, but the source was not deleted: { $error }
ops-move-size = Copy is { $got } B instead of { $want } B — source kept
ops-move-flush = Copy not flushed to disk: { $error } — source kept
ops-copy-job-failed = Operation failed: { $error }
ops-copy-errors = { $n ->
    [one] 1 item failed
   *[other] { $n } items failed
}: { $first }
ops-conflict-exists = File already exists
ops-conflict-other = Could not process the file
ops-conflict-body = Conflict { $index } of { $total }: { $name } — { $cmp }
ops-conflict-body-short = Conflict { $index } of { $total }: { $name }
ops-conflict-newer = newer
ops-conflict-older = older
ops-conflict-same-time = same time
ops-conflict-larger = larger by { $size }
ops-conflict-smaller = smaller by { $size }
ops-conflict-same-size = same size
ops-issue-body = Problem { $index } of { $total }: { $name } — { $error }
ops-issue-permission = Access denied
ops-issue-sharing = File is in use by another program
ops-issue-busy = Device is busy or not ready
ops-issue-network = Network is unavailable
ops-issue-not-found = Not found
ops-issue-other = Operation failed
ops-issue-low-space = Not enough disk space
ops-choice-retry = Retry
ops-retry-nothing = Nothing to retry: no errors, or the task report is no longer kept
ops-recover-banner = Interrupted: { $what } → { $dest }
ops-recover-banner-pct = Interrupted: { $what } → { $dest } (≈{ $n }% done)
ops-recover-continue = Continue
ops-recover-clear = Clear
ops-quit-title = Operations in progress ({ $n })
ops-quit-body = “Abort and exit” stops the tasks; you can continue them on the next start
ops-quit-wait = Wait
ops-quit-abort = Abort and exit
ops-choice-overwrite = Replace
ops-choice-skip = Skip
ops-choice-rename = Rename
ops-choice-newer = Replace if newer
ops-choice-cancel = Cancel
ops-copy-skipped = { $n ->
    [one] 1 item skipped
   *[other] { $n } items skipped
}
ops-open-failed = Cannot open { $name }: { $error }
core-settings-not-saved = Settings not saved to { $file }: { $error }
ops-mkdir-failed = Cannot create { $name }: { $error }
ops-rename-failed = Cannot rename { $name }: { $error }
ops-exists = { $name } already exists
ops-rename-bad = A name cannot contain \ or / — use Move (F6) to put it elsewhere
ops-job-trash = Moving to Recycle Bin
ops-job-delete = Deleting
ops-delete-failed = Not deleted: { $error }
