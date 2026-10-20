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
ops-copy-job-failed = Operation failed: { $error }
ops-copy-errors = { $n ->
    [one] 1 item failed
   *[other] { $n } items failed
}: { $first }
ops-copy-asked = { $n ->
    [one] 1 file already exists and was skipped
   *[other] { $n } files already exist and were skipped
} (replace prompt is not available yet)
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
