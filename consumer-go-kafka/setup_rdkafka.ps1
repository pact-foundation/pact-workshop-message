# Install the librdkafka.redist package version 2.13.3
nuget install librdkafka.redist -Version 2.13.3

Copy-Item -Recurse $pwd\librdkafka.redist.2.13.3\runtimes\win-x64\native\* $env:TMP

$goPath = & go env GOPATH
Remove-Item -Force "$goPath/pkg/mod/github.com/confluentinc/confluent-kafka-go/v2@v2.13.3/kafka/librdkafka_vendor/librdkafka_windows.a"

# Copy the librdkafka.dll to the specified directory
Copy-Item .\librdkafka.redist.2.13.3\build\native\lib\win\x64\win-x64-Release\v142\librdkafka.lib "$goPath/pkg/mod/github.com/confluentinc/confluent-kafka-go/v2@v2.13.3/kafka/librdkafka_vendor/librdkafka_windows.a"
