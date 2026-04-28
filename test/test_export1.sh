set -e

EXPORT1="export1"
EXPORT1_EXT="export1.img"
MNT_EXPORT1="mnt/export1"
FILE="mnt/export1/export1.txt"

# mc path
export PATH=$PATH:~/aistor-binaries/

# removes the s3 storage, but okay if there's nothing to remove
mc rm --force --recursive minio/howard/${EXPORT1_EXT} || true
rm -f ${EXPORT1_EXT} || true

output=$(sudo nbd-client localhost 10809 -N ${EXPORT1})
echo "${output}"
device1=$(echo "${output}" | grep "Connected" | awk '{printf $2}')

sudo mkfs.ext4 "${device1}"

if ! [ -d $MNT_EXPORT1 ]; then
    sudo mkdir $MNT_EXPORT1
fi
sudo mount "${device1}" "${MNT_EXPORT1}"
sudo chown -R $USER:$USER $MNT_EXPORT1
MAX=100
for i in $(seq 1 $MAX); do
    echo "iter $i"
    echo "writing to ${FILE}"
    echo "export2" > $FILE
    echo "reading from ${FILE}"
    grep -q "export2" "${FILE}"
done
sudo umount "${MNT_EXPORT1}"
sudo nbd-client -d "${device1}"