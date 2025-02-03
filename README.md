# diffpatch

When running diffpatch on two folders
```sh
diffpatch path/to/before path/to/after
```
it will display a `git add -p` like UI where you can interactively stage changes from before to after.
At the end, only the added changes will be materialized in `after`.

In other words, if you say `y` to everything the final state will be `right`, and if you say `n` the state will be `left`.

## Integration

### Jujutsu

```toml
[ui]
diff-editor = "diffpatch"
```

will specify `diffpatch` as the default diff editor, which will be used for `jj commit`, `jj restore`, `jj split`, `jj squash` and `jj diffedit`.
