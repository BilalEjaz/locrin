<?php

// Copyright 2026 Example Ltd
// SPDX-License-Identifier: MIT
// All rights reserved.

/**
 * Loads the thing.
 * Returns a number.
 * Never throws.
 */
function load(array $rows): int
{
    # It accepts a name, an id,
    # and an optional callback, described below.
    # The callback runs once the fetch settles.
    return count($rows);
}
