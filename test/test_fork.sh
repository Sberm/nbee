# ===========================
# |   (C) 2025 Howard Chu   |
# ===========================
#   o
#    o
#      o
#         /\      /\
#       -/--\----/--\-
#      /  /\      /\  \
#     /  (())    (())  \
#     \   \/      \/   /
#      \    (_/\_)    /
#       --------------
#
# To run this test, do `sh test/test_fork.sh` in the root directory
# Also run this test while the server is running (cargo run).
#
# This test forks from "test_device.img"

# mc path
export PATH=$PATH:~/aistor-binaries/

NAME_FROM="test_device"
NAME_FROM_W_EXT="${NAME_FROM}.img"
NAME="test_device_fork"
NAME_W_EXT="${NAME}.img"

DIR="/mnt/howard"
FILE="/mnt/howard/foobar.txt"
FILE_CONTENT="foobar"

set -e

exists_s3() {
    echo "checking if the object exists"
    if mc ls minio/howard | grep -q "${NAME_W_EXT}"; then
        return 0
    else
        return 1
    fi
}

exists_tmp_storage() {
    echo "checking if the tmp file exists"
    if [ -f ${NAME_W_EXT} ]; then
        return 0
    else
        return 1
    fi
}

# removes the s3 storage, but okay if there's nothing to remove
mc rm --force --recursive minio/howard/${NAME_W_EXT} || true
rm -f ${NAME_W_EXT} || true
mc rm --force --recursive minio/howard/${NAME_FROM_W_EXT} || true
rm -f ${NAME_FROM_W_EXT} || true

# writes to original
echo "writing to the original"
output=$(sudo nbd-client localhost 10809 -N ${NAME_FROM})
echo "${output}"
device=$(echo "${output}" | grep "Connected" | awk '{printf $2}')
echo "device: ${device}"
# mkfs one time should be fine
sudo mkfs.ext4 "${device}"
echo "mounting original"
sudo mount "${device}" /mnt
# writes to a file
if ! [ -d "${DIR}" ]; then
    sudo mkdir "$DIR"
fi
sudo chown -R $USER:$USER $DIR
touch "${FILE}"
echo "${FILE_CONTENT}" > "${FILE}"
echo "detaching original"
sudo umount /mnt
# sleep so the server is able to write all the remaining data, before disconnecting
sleep 2
sudo nbd-client -d "${device}"


# create a readonly fork using the "_fork" suffix, should have the same content
# as original.
# but there shouldn' be separate s3 storage and temporary file, because copy-on-write
echo "creating a readonly fork"
output=$(sudo nbd-client localhost 10809 -N ${NAME} -R)
echo "${output}"
device=$(echo "${output}" | grep "Connected" | awk '{printf $2}')
echo "device: ${device}"
echo "mounting readonly fork"
sudo mount "${device}" /mnt
echo "Accessing the readonly fork"
if ! cat "${FILE}" | grep -q "${FILE_CONTENT}"; then
    echo "readonly fork doesn't have matching content"
    exit -1
fi
if exists_s3 ; then
    echo "${NAME_W_EXT} exists in s3, but it shouldn't"
    exit -1
fi
if exists_tmp_storage; then
    echo "${NAME_W_EXT} exists in temp storage when it shouldn't"
    exit -1
fi
echo "detaching readonly fork"
sudo umount /mnt
sleep 2
sudo nbd-client -d "${device}"


# creates another fork
# this fork writes to file, so there should be separate temp file and s3 entry
echo "creating a writeable fork"
output=$(sudo nbd-client localhost 10809 -N ${NAME})
echo "${output}"
device=$(echo "${output}" | grep "Connected" | awk '{printf $2}')
echo "device: ${device}"
echo "mounting writeable fork"
sudo mount "${device}" /mnt
echo "Accessing the writeable fork"
if ! cat "${FILE}" | grep -q "${FILE_CONTENT}"; then
    echo "writeable fork doesn't have previous content"
    exit -1
fi
# writes more content to file
echo "more foobar" >> "${FILE}"
if ! exists_s3 ; then
    echo "${NAME_W_EXT} doesn't exist in s3, but it should"
    exit -1
fi
if ! exists_tmp_storage; then
    echo "${NAME_W_EXT} doesn't exist in temp storage when it should"
    exit -1
fi
echo "detaching writeable fork"
sudo umount /mnt
sleep 2
sudo nbd-client -d "${device}"

printf "\033[0;32m[PASSED]\033[0m\n"