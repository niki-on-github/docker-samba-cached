#!/bin/bash

if [[ -z "$CACHE_WATCH_DIR" ]]; then
    echo "CACHE_WATCH_DIR is not set. Disable cache."
    sleep 300
    exit
fi

MAX_CACHE_FILES=${MAX_CACHE_FILES:-2}

# Array to keep track of the LRU queue
declare -a CACHED_FILES

echo "Starting Media RAM Cacher..."

# Listen for 'open' events on files
inotifywait -m -e open --format "%w%f" -r "$WATCH_DIR" | while read -r FILE
do
    # Only react to video files to ignore tiny metadata files
    if [[ "$FILE" =~ \.(mkv|mp4|avi)$ ]]; then
        
        # Check if the file is already in our cache list (debounce)
        ALREADY_CACHED=0
        for cached in "${CACHED_FILES[@]}"; do
            if [[ "$cached" == "$FILE" ]]; then
                ALREADY_CACHED=1
                break
            fi
        done

        if [[ $ALREADY_CACHED -eq 0 ]]; then
            echo "New video opened: $FILE"
            
            # 1. Force the file into RAM in the background
            # -t touches it into memory, -b runs it in background so script doesn't hang
            vmtouch -t -b "$FILE"

            # 2. Add to our queue
            CACHED_FILES+=("$FILE")
            
            # 3. LRU Eviction: If we have more than MAX_CACHE_FILES
            if [[ ${#CACHED_FILES[@]} -gt $MAX_CACHE_FILES ]]; then
                # Get the oldest file (index 0)
                OLDEST_FILE="${CACHED_FILES[0]}"
                echo "Cache full. Evicting oldest file from RAM: $OLDEST_FILE"
                
                # Forcefully evict the oldest file from RAM (-e)
                vmtouch -e "$OLDEST_FILE"
                
                # Remove oldest file from the array
                CACHED_FILES=("${CACHED_FILES[@]:1}")
            fi
        fi
    fi
done
