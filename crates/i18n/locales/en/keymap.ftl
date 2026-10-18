# Keymap (S3-10): banner and keymap.dump
keymap-warnings = Key bindings: { $count } { $count ->
    [one] problem
   *[other] problems
}. First: { $first }
keymap-kept = Key bindings not reloaded, the previous ones are kept: { $first }
keymap-dumped = Key bindings saved to { $path }
keymap-dump-failed = Could not save key bindings to { $path }: { $error }
