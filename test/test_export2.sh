set -e

EXPORT2="export2"
EXPORT2_EXT="export2.img"
MNT_EXPORT2="mnt/export2"
FILE="mnt/export2/export2.txt"

# mc path
export PATH=$PATH:~/aistor-binaries/

# removes the s3 storage, but okay if there's nothing to remove
mc rm --force --recursive minio/howard/${EXPORT2_EXT} || true
rm -f ${EXPORT2_EXT} || true

output=$(sudo nbd-client localhost 10809 -N ${EXPORT2})
echo "${output}"
device2=$(echo "${output}" | grep "Connected" | awk '{printf $2}')

sudo mkfs.ext4 "${device2}"

if ! [ -d $MNT_EXPORT2 ]; then
    sudo mkdir $MNT_EXPORT2
fi
sudo mount "${device2}" "${MNT_EXPORT2}"
sudo chown -R $USER:$USER $MNT_EXPORT2
MAX=100
for i in $(seq 1 $MAX); do
    echo "iter $i"
    echo "writing to ${FILE}"
    echo "export2" > $FILE
    echo "reading from ${FILE}"
    grep -q "export2" "${FILE}"
done
sudo umount "${MNT_EXPORT2}"
sudo nbd-client -d "${device2}"