find . -name "*.rs" | while read fname; do
  echo "$fname"
  python3 ../../tools/extract_array_indexing_op.py $fname
done