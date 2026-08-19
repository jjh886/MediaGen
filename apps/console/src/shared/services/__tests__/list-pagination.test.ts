import { describe, expect, it } from 'vitest'
import { appendUnique, LIST_PAGE_SIZE, pagedQuery } from '../list-pagination'

describe('list-pagination', () => {
  it('默认生成四十条的第一页参数', () => {
    expect(pagedQuery().toString()).toBe(`limit=${LIST_PAGE_SIZE}&offset=0`)
    expect(pagedQuery(20, 40).toString()).toBe('limit=20&offset=40')
  })

  it('追加下一页时保留新数据并去掉重复主键', () => {
    const merged = appendUnique([{ id: '1' }], [{ id: '1' }, { id: '2' }], (item) => item.id)

    expect(merged).toEqual([{ id: '1' }, { id: '2' }])
  })
})
