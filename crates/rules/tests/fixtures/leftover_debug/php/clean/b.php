<?php
function total(array $rows): int
{
    error_log("counting rows");
    echo count($rows);
    printf("%d\n", count($rows));
    $logger->debug("counted");
    Acme\dump($rows);
    $text = print_r($rows, true);
    $code = var_export($rows, true);
    $named = print_r($rows, return: true);
    return count($rows);
}
