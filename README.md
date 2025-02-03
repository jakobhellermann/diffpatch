# diffpatch

[![asciicast demo](https://asciinema.org/a/701405.svg)](https://asciinema.org/a/701405)

When running diffpatch on two folders
```sh
diffpatch path/to/before path/to/after
```
it will display a `git add -p` like UI where you can interactively stage changes from before to after.
At the end, only the added changes will be materialized in `after`.

In other words, if you say `y` to everything the final state will be `right`, and if you say `n` the state will be `left`.

## Integration

```sh
cargo install --git https://github.com/jakobhellermann/diffpatch
```

### Jujutsu

```toml
[ui]
diff-editor = "diffpatch"
```
in `.config/jj/config.toml` will specify `diffpatch` as the default diff editor, which will be used for `jj commit`, `jj restore`, `jj split`, `jj squash` and `jj diffedit`.


## Configuration

So far, `diffpatch` can only be configured through environment variables:

- `DIFFPATCH_IMMEDIATE_COMMAND` (`=true`) When set, you can type `[y,n,q,a,d,e]` immediately without pressing enter.

- `DIFFPATCH_INTERFACE`
  - `direct` (default) Directly write changes to the terminal. Matches the behaviour of `git add -p`.
  - `fullscreen` Go into fullscreen and display the changes there. Upon exit, the terminal will be restored to its previous state.
  - `inline-clear` Don't go to fullscreen, but clear written lines after each hunk. (experimental)

- `DIFFPATCH_CONTEXT_LEN`: (`=3`) The amount of context lines that are displayed around each change.
