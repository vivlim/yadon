# yadon

![picture of the Pokémon Slowpoke, whose original Japanese name is Yadon](https://raw.githubusercontent.com/vivlim/yadon/main/079.png)

*why write now, when you can write later?*

`Yadon` is a struct which implements `Write + Seek` but doesn't actually write anything until later, when you call `.apply()`.

## why

~~yes~~ I was trying to push a generic write operation using [binrw](https://github.com/jam1garner/binrw) through a channel to be actually performed on another thread, and being able to store the *result* of the write operation meant I sidestepped some particularly hairy issues where I would have had to store trait objects which had an associated generic function - impossible since having that associated generic function [made the entire trait not 'object safe'.](https://stackoverflow.com/questions/42620022/why-does-a-generic-method-inside-a-trait-require-trait-object-to-be-sized)