<?php

// VULOPILOT_VULOCLOUD_URL is the VuloCloud platform's own API base,
// the same for every connection, so it is read from the environment once.
// $config['domain'], which is this connection's own organisation host, is
// read per connection and never falls back to the platform base.
function base(): int
{
    return 1;
}
