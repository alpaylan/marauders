# Insert
def insert(k: int, v: int, t: BST) -> BST:
    match t:
        case E():
            return _node(E(), k, v, E())
        case T(k2, v2, l, r):
            """! insert """
            if k < k2:
                return _node(insert(k, v, l), k2, v2, r)
            elif k2 < k:
                return _node(l, k2, v2, insert(k, v, r))
            else:
                return _node(l, k2, v, r)
            """!! insert_1 """
            """!
            return _node(E(), k, v, E())
            """
            """!! insert_2 """
            """!
            if k < k2:
                return _node(insert(k, v, l), k2, v2, r)
            else:
                return _node(l, k2, v, r)
            """
            """!! insert_3 """
            """!
            if k < k2:
                return _node(insert(k, v, l), k2, v2, r)
            elif k2 < k:
                return _node(l, k2, v2, insert(k, v, r))
            else:
                return _node(l, k2, v2, r)
            """
            """ !"""
