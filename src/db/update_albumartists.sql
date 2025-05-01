-- This query updates the albumartist column for all tracks in the tracks table
-- where albumartist is NULL. It sets the albumartist to the name of a single artist
-- that is present on all tracks in the album, or to '[Various Artists]' if there isn't one.
UPDATE tracks SET albumartist = CASE WHEN (
  SELECT COUNT(*) FROM (
    SELECT a1.name FROM artists a1 
      JOIN tracks t1 ON a1.path = t1.path 
      WHERE t1.album = tracks.album 
      GROUP BY a1.name 
      HAVING COUNT(DISTINCT t1.path) = (
        SELECT COUNT(*) FROM tracks t2 WHERE t2.album = tracks.album
      )
    )
  ) = 1 THEN (
    SELECT a.name FROM artists a 
      JOIN tracks t ON a.path = t.path
      WHERE t.album = tracks.album 
      GROUP BY a.name 
      HAVING COUNT(DISTINCT t.path) = (
        SELECT COUNT(*) FROM tracks t2 WHERE t2.album = tracks.album
      ) 
    LIMIT 1
  ) ELSE :various_artists END 
WHERE albumartist IS NULL;
