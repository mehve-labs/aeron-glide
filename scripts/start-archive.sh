#!/bin/bash
set -e

# The Archive must match the Aeron version the client was built against, and
# target/ accumulates jars from every version ever built. Pin the lookup to the
# version in build.rs (overridable the same way the build is).
AERON_VERSION="${AERON_VERSION:-$(sed -n 's/.*AERON_VERSION").unwrap_or_else(|_| "\([0-9.]*\)".*/\1/p' build.rs)}"

if [ -z "$AERON_VERSION" ]; then
    echo "ERROR: could not determine the Aeron version from build.rs."
    echo "Set AERON_VERSION explicitly, e.g. AERON_VERSION=1.53.3 $0"
    exit 1
fi

JAR=$(find target -name "aeron-all-${AERON_VERSION}.jar" -print -quit 2>/dev/null)

if [ -z "$JAR" ]; then
    echo "ERROR: aeron-all-${AERON_VERSION}.jar not found."
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
