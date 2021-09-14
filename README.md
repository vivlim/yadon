# biding-writer

*why write now, when you can write later?*

`BidingWriter` is a struct which implements `Write + Seek` but doesn't actually write anything until later, when you call `.apply()`.

## why

yes