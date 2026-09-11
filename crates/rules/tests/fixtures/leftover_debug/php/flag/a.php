<?php
function total(array $rows): int
{
    var_dump($rows);
    print_r($rows);
    var_export($rows);
    dd($rows);
    dump($rows);
    debug_zval_dump($rows);
    xdebug_break();
    \var_dump($rows);
    print_r($rows, false);
    var_export($rows, $flag);
    return count($rows);
}
