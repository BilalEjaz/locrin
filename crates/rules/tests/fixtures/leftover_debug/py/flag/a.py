import pdb
import ipdb
from pdb import set_trace


def total(rows):
    breakpoint()
    pdb.set_trace()
    ipdb.set_trace()
    pudb.set_trace()
    return len(rows)
