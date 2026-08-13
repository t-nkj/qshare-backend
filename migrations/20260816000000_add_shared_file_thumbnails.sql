ALTER TABLE `shared_files`
    ADD COLUMN `thumbnail_content_type` VARCHAR(255) NULL AFTER `storage_key`,
    ADD COLUMN `thumbnail_data` MEDIUMBLOB NULL AFTER `thumbnail_content_type`;
