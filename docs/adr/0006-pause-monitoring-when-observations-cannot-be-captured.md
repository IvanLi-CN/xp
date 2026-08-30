# Pause monitoring when observations cannot be captured

Service Monitoring starts a due check only when its Source can durably accept
the resulting Observation for History Repository delivery. A temporary
repository outage may accumulate a Recoverable Backlog in the Source Delivery
Journal, but a Source Capture Suspension pauses new checks and leaves their
Schedule Slots visibly uncovered. Continuing to probe while discarding results
was rejected because it would present an apparently healthy monitoring system
with unreported evidence; ordinary-node long-term fallback storage was rejected
because it would create a second retention and repair contract.
