<?php
function total(array $rows): int
{
    // $old = compute($rows);
    // foreach ($rows as $row) {
    //     $old += $row;
    // }
    return count($rows);
}

function legacy(): int
{
    # $legacy = 1;
    # $more = 2;
    # return $legacy;
}

/*
$dead = 1;
$gone = 2;
return $dead;
*/
