import logging

logger = logging.getLogger(__name__)


def total(rows):
    print("counting rows")
    print(len(rows))
    logging.debug("counted")
    logger.debug("counted")
    return len(rows)
