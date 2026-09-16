<?php

// ACTION: ajouter screenpal.com et media.memora.solutions (CNAME ScreenPal)
// au frame-src, puis le domaine du lecteur au media-src.
// SELF: edition 1 ligne. RAISON: SecurityHeaders est la CSP globale active,
// qui ecrase AcademyCsp sur toutes les pages du module.
// $csp doit rester la seule source de verite pour les pages du module.
function boot(): int
{
    return 1;
}
