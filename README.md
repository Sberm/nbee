run server

```sh
cargo run
```

test

```sh
$ sudo nbd-client localhost 10809 -N test_device
Negotiation: ..size = 1024MB
Connected /dev/nbd3

$ sudo mkfs.ext4 /dev/nbd3
mke2fs 1.47.2 (1-Jan-2025)
Creating filesystem with 262144 4k blocks and 65536 inodes
Filesystem UUID: 73890a37-128a-4c64-aa15-e6a18ee695ef
Superblock backups stored on blocks:
        32768, 98304, 163840, 229376

Allocating group tables: done
Writing inode tables: done
Creating journal (8192 blocks): done
Writing superblocks and filesystem accounting information: done


$ sudo mount /dev/nbd3 /mnt

$ # test out filesystem by reading & writing files to /mnt

...

# Cleanup
$ sudo umount /mnt

$ sudo nbd-client -d /dev/nbd3
```

S3 storage start
```sh
minio server minio
```

Clear nbd connections
```sh
for i in {0..30}; do sudo nbd-client -d /dev/nbd${i}; done
```