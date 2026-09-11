<?php
function total(array $rows): int
{
    error_log("counting rows");
    echo count($rows);
    printf("%d\n", count($rows));
    $logger->debug("counted");
    return count($rows);
}
