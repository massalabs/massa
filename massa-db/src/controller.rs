use crate::DBBatch;

pub trait MassaCF {
    fn write_batch(&self, batch: DBBatch); /*{
                                               let db = self.db.read();
                                               let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
                                               batch.write_batch.put_cf(
                                                   handle,
                                                   ASYNC_POOL_HASH_KEY,
                                                   batch
                                                       .async_pool_hash
                                                       .expect(WRONG_BATCH_TYPE_ERROR)
                                                       .to_bytes(),
                                               );
                                               db.write(batch.write_batch).expect(CRUD_ERROR);
                                           }*/
}
