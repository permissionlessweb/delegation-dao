#!/bin/bash

# Check if input file is provided
if [ $# -ne 1 ]; then
    echo "Usage: $0 <input_csv_file>"
    exit 1
fi

input_file=$1

# Check if input file exists
if [ ! -f "$input_file" ]; then
    echo "Error: Input file not found!"
    exit 1
fi

# Create JSON structure
echo "{" > output.json
echo "  \"validators\": [" >> output.json

# Read the CSV file and convert to JSON
awk -F',' '
    BEGIN { OFS=""; }
    {
        if (NR > 1) {
            print "    ," >> "output.json"
        }
        print "    {" >> "output.json"
        print "      \"address\": \"" $1 "\"," >> "output.json"
        print "      \"amount\": " $2 >> "output.json"
        print "    }" >> "output.json"
    }
' "$input_file"

# Close JSON structure
echo "  ]" >> output.json
echo "}" >> output.json

echo "Conversion completed. Output saved to output.json"