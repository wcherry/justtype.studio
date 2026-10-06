#!/bin/sh
# A volume mounted from the host arrives owned by whoever made it; the service
# runs as appuser, so hand it the data before dropping root.
chown -R appuser:appuser /usr/local/data
exec gosu appuser "$@"
