#!/bin/bash
set -e

# Find the aeron-all jar built by cargo build --features archive
JAR=$(find target -name "aeron-all-*.jar" -print -quit 2>/dev/null)

if [ -z "$JAR" ]; then
    echo "ERROR: aeron-all jar not found."
    echo "Run 'JAVA_HOME=/path/to/jdk17+ cargo build --features archive' first."
    exit 1
fi

echo "Using jar: $JAR"
echo "Starting Aeron ArchivingMediaDriver..."
echo "  Control channel: aeron:udp?endpoint=localhost:8010"
echo "  Press Ctrl+C to stop."
echo ""

exec java \
    --add-opens java.base/jdk.internal.misc=ALL-UNNAMED \
    --add-opens java.base/sun.nio.ch=ALL-UNNAMED \
    -Daeron.dir.delete.on.start=true \
    -Daeron.archive.dir.delete.on.start=true \
    -Daeron.archive.control.channel=aeron:udp?endpoint=localhost:8010 \
    -Daeron.archive.control.stream.id=10 \
    -Daeron.archive.control.response.channel=aeron:udp?endpoint=localhost:0 \
    -Daeron.archive.control.response.stream.id=20 \
    -Daeron.archive.replication.channel=aeron:udp?endpoint=localhost:0 \
    -Daeron.print.configuration=true \
    -cp "$JAR" \
    io.aeron.archive.ArchivingMediaDriver
